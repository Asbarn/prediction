# Phase 39: Grafana Dashboards and Alert Rules - Research

**Researched:** 2026-03-08
**Domain:** Grafana OSS dashboard provisioning, alerting rules, PromQL queries
**Confidence:** HIGH

## Summary

This phase creates five operational Grafana dashboards and critical alert rules, all provisioned via Grafana's file-based provisioning system. The infrastructure is already in place: self-hosted Grafana OSS 11.5.2 runs on EC2 via docker-compose, with a pre-provisioned AMP data source providing access to 80+ application Prometheus metrics. The provisioning directory (`grafana/provisioning/`) is already mounted into the Grafana container and used for the data source configuration.

Grafana's file-based provisioning supports three resource types: data sources (already done), dashboards (via JSON files referenced by a YAML provider config), and alerting resources (alert rules, contact points, notification policies -- all via YAML files in `provisioning/alerting/`). All provisioned resources are loaded at Grafana startup and can be hot-reloaded via the admin API.

**Primary recommendation:** Create dashboard JSON files and alerting YAML files in the `grafana/provisioning/` directory, add a dashboard provider YAML config, and update the CDK user-data to write these files to the EC2 instance. Set `allowUiUpdates: true` on the dashboard provider so dashboards can be tweaked in the Grafana UI during iterative development.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MON-04 | Grafana dashboard: Feed Health (feed_available per venue, reconnection rate, message latency) | Metrics available: `feed_available`, `feed_reconnections_total`, `feed_last_latency_ms`, `feed_latency_ms`, `feed_messages_total`, `feed_heartbeat_timeouts`. All have `venue` label for per-venue breakdown. |
| MON-05 | Grafana dashboard: Signal Quality (arb_signals_emitted, net_edge_bps, confidence, staleness rejections) | Metrics available: `arb_signals_emitted_total`, `arb_staleness_rejections`, `arb_computations_total`, `arb_signals_filtered_total`, `spread_staleness_rejections`, `spread_signals_total`. Note: `net_edge_bps` and `confidence` are not directly exposed as metrics -- research shows these are logged but not metered. Dashboard should use available signal metrics. |
| MON-06 | Grafana dashboard: Paper Trade P&L (daily_pnl, win_rate, net_pnl, settlement latency) | Metrics available: `paper_trade_daily_pnl`, `paper_trade_daily_win_rate`, `paper_trade_daily_trades`, `paper_trades_total`, `paper_trades_open`, `paper_trades_settled_total`, `settlement_timeouts_total`, `settlement_outcomes_total`. |
| MON-07 | Grafana dashboard: System Health (active expiries, subscriptions, lifecycle polls, proposals, alerts) | Metrics available: `pricing_active_expiries`, `subscription_active`, `lifecycle_discovery_polls`, `proposals_total`, `proposals_pending`, `alert_monitor_active_alerts`, `alert_active`. |
| MON-08 | Grafana alert rules for: feed down 5min, zero spread computations 30min, high staleness rejection rate | Metrics: `feed_available` (gauge per venue, fires when 0 for 5m), `spread_computations_total` (counter, fires when rate is 0 for 30m), `arb_staleness_rejections` / `arb_computations_total` (ratio threshold). |
</phase_requirements>

## Standard Stack

### Core

| Component | Version | Purpose | Why Standard |
|-----------|---------|---------|--------------|
| grafana/grafana-oss | 11.5.2 | Dashboard visualization and alerting | Already deployed in Phase 37 |
| Grafana file provisioning | Built-in | Automated dashboard/alert deployment | No external tools needed; YAML/JSON files in provisioning directory |
| AMP (Amazon Managed Prometheus) | Managed service | Metrics backend | Already deployed in Phase 37 |
| PromQL | Standard | Query language for dashboards and alerts | Native to Prometheus/AMP data source |

### Supporting

| Component | Purpose | When to Use |
|-----------|---------|-------------|
| Grafana Admin API (`/api/admin/provisioning/dashboards/reload`) | Hot-reload provisioned dashboards without restart | After updating dashboard JSON files |
| Grafana Admin API (`/api/admin/provisioning/alerting/reload`) | Hot-reload alerting provisioning | After updating alerting YAML files |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| File provisioning | Grafana HTTP API | More complex; file provisioning is simpler for static dashboards |
| File provisioning | Terraform Grafana provider | Overkill for single instance; project already uses CDK |
| JSON dashboard files | Grafana UI export | UI-created dashboards aren't version controlled; file provisioning is code-as-config |

## Architecture Patterns

### Recommended Project Structure

```
grafana/
  provisioning/
    datasources/
      amp.yml                    # Already exists (Phase 37)
    dashboards/
      provider.yml               # Dashboard provider config (points to JSON dir)
      json/
        feed-health.json         # MON-04
        signal-quality.json      # MON-05
        paper-trade-pnl.json     # MON-06
        system-health.json       # MON-07
    alerting/
      rules.yml                  # Alert rules (MON-08)
      contact-points.yml         # Contact point definitions
      notification-policies.yml  # Routing rules
```

### Pattern 1: Dashboard Provider Configuration

**What:** YAML file that tells Grafana where to find dashboard JSON files.
**When to use:** Always required for file-based dashboard provisioning.

```yaml
# grafana/provisioning/dashboards/provider.yml
apiVersion: 1

providers:
  - name: prediction-dashboards
    orgId: 1
    type: file
    disableDeletion: false
    allowUiUpdates: true
    updateIntervalSeconds: 30
    options:
      path: /etc/grafana/provisioning/dashboards/json
      foldersFromFilesStructure: false
```

Key fields:
- `allowUiUpdates: true` -- lets you tweak dashboards in the UI. Changes persist to Grafana DB. When the JSON file changes and Grafana reloads, it overwrites UI changes.
- `updateIntervalSeconds: 30` -- how often Grafana checks for file changes.
- `path` -- must match the container mount path.

### Pattern 2: Dashboard JSON with Data Source Reference

**What:** Reference the AMP data source by name (not UID) for portability.
**When to use:** In every panel's datasource field.

```json
{
  "datasource": {
    "type": "prometheus",
    "uid": "amp"
  },
  "targets": [
    {
      "expr": "feed_available",
      "legendFormat": "{{venue}}",
      "refId": "A"
    }
  ]
}
```

**IMPORTANT:** The AMP data source provisioning (`amp.yml`) currently does not set a `uid` field. Add `uid: amp` to the data source provisioning YAML so dashboards can reference it consistently. Without a stable UID, Grafana auto-generates one that changes between deployments.

Updated `amp.yml`:
```yaml
apiVersion: 1

datasources:
  - name: AMP
    uid: amp
    type: prometheus
    access: proxy
    url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/${AMP_WORKSPACE_ID}/
    isDefault: true
    jsonData:
      httpMethod: POST
      sigV4Auth: true
      sigV4AuthType: default
      sigV4Region: us-east-1
    editable: true
```

### Pattern 3: Alert Rule with PromQL Query

**What:** Two-step evaluation: PromQL query (refId A) + threshold expression (refId B using `__expr__`).
**When to use:** All Grafana-managed alert rules.

```yaml
apiVersion: 1
groups:
  - orgId: 1
    name: feed-alerts
    folder: Prediction Alerts
    interval: 1m
    rules:
      - uid: feed-down-alert
        title: Feed Down
        condition: B
        for: 5m
        noDataState: Alerting
        execErrState: Alerting
        data:
          - refId: A
            relativeTimeRange:
              from: 600
              to: 0
            datasourceUid: amp
            model:
              editorMode: code
              expr: min(feed_available) by (venue)
              instant: true
              intervalMs: 1000
              maxDataPoints: 43200
              refId: A
          - refId: B
            datasourceUid: __expr__
            model:
              conditions:
                - evaluator:
                    params:
                      - 1
                    type: lt
                  operator:
                    type: and
                  query:
                    params:
                      - B
                  reducer:
                    params: []
                    type: last
                  type: query
              datasource:
                type: __expr__
                uid: __expr__
              expression: A
              intervalMs: 1000
              maxDataPoints: 43200
              refId: B
              type: threshold
        annotations:
          summary: "Feed {{ $labels.venue }} has been down for 5 minutes"
        labels:
          severity: critical
```

### Pattern 4: Dashboard JSON Skeleton

**What:** Minimal dashboard JSON structure with required fields.

```json
{
  "uid": "feed-health",
  "title": "Feed Health",
  "tags": ["prediction", "feeds"],
  "timezone": "utc",
  "editable": true,
  "fiscalYearStartMonth": 0,
  "graphTooltip": 1,
  "panels": [],
  "time": { "from": "now-1h", "to": "now" },
  "refresh": "30s",
  "schemaVersion": 39,
  "version": 1
}
```

Key fields:
- `uid` -- stable identifier for the dashboard; must be unique.
- `schemaVersion` -- use 39 for Grafana 11.x.
- `refresh: "30s"` -- auto-refresh interval for real-time monitoring.
- `graphTooltip: 1` -- shared crosshair across panels (useful for correlating metrics).

### Anti-Patterns to Avoid

- **Auto-generated data source UIDs in dashboard JSON:** Always set an explicit `uid` on the data source provisioning and reference that in dashboards. Auto-generated UIDs break on redeployment.
- **Overly complex dashboards:** Keep each dashboard focused on one domain. Five focused dashboards are better than one mega-dashboard.
- **Alert rules without `for` duration:** Alerts without a `for` period fire on transient spikes. Always set a meaningful `for` duration.
- **Using `rate()` on gauges:** Metrics like `feed_available` are gauges, not counters. Use `rate()` only on `_total` counter metrics.
- **Hardcoding panel positions:** Use `gridPos` consistently. Each panel needs `h`, `w`, `x`, `y` coordinates. Standard widths: 24 (full), 12 (half), 8 (third), 6 (quarter).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Dashboard JSON creation | Write JSON from scratch | Build in Grafana UI, then export as JSON | Grafana panel builder handles gridPos, field config, and visualization options correctly |
| Alert evaluation logic | Custom alerting code | Grafana Alerting with `__expr__` conditions | Built-in evaluation, state management, and notification routing |
| Metric aggregation | Application-side pre-aggregation | PromQL `rate()`, `sum()`, `avg()` | Standard, composable, and query-time flexible |

**Key insight:** The most practical approach is to build dashboards interactively in the Grafana UI, export the JSON, and commit the JSON files to the repo for provisioning. This combines the ease of visual building with version-controlled infrastructure-as-code.

## Common Pitfalls

### Pitfall 1: Data Source UID Mismatch
**What goes wrong:** Dashboard panels show "No data" or "Data source not found" after redeployment.
**Why it happens:** If the data source provisioning YAML does not specify a `uid`, Grafana auto-generates one. Dashboard JSON files reference a UID that no longer exists after Grafana container recreation.
**How to avoid:** Add `uid: amp` to `grafana/provisioning/datasources/amp.yml`. Reference `"uid": "amp"` in all dashboard panel datasource fields.
**Warning signs:** Panels showing "Datasource not found" errors after container restart.

### Pitfall 2: Provisioning Directory Not Mounted
**What goes wrong:** New provisioning files (dashboards, alerting) are not loaded by Grafana.
**Why it happens:** The docker-compose volume mount (`./grafana/provisioning:/etc/grafana/provisioning:ro`) maps the local directory. If new subdirectories (dashboards/json, alerting) are not created in the user-data, they won't exist in production.
**How to avoid:** The CDK user-data must create all provisioning subdirectories AND write all provisioning files before starting docker-compose. Update user-data to write dashboard provider YAML, dashboard JSON files, and alerting YAML files.
**Warning signs:** Grafana logs showing "no provisioning files found" or missing directories.

### Pitfall 3: Alert Rule Folder Must Exist
**What goes wrong:** Alert rule provisioning fails with "folder not found" error.
**Why it happens:** Grafana alerting provisioning references a `folder` name. If the folder doesn't exist, Grafana creates it automatically for alerting -- but this is a common source of confusion.
**How to avoid:** Use a descriptive folder name (e.g., "Prediction Alerts") in the alert rules YAML. Grafana will auto-create it.
**Warning signs:** Grafana startup logs showing provisioning errors.

### Pitfall 4: Rate Queries on Zero-Value Counters
**What goes wrong:** `rate(spread_computations_total[30m])` returns nothing instead of 0 when no computations occur.
**Why it happens:** If the counter doesn't exist in the series (application hasn't emitted it yet), `rate()` returns empty, not 0. Also, `rate()` on a non-incrementing counter returns 0, which IS what we want for the "zero computations" alert.
**How to avoid:** For the "zero spread computations for 30 minutes" alert, use `rate(spread_computations_total[30m]) == 0` or `absent_over_time(spread_computations_total[30m])` to handle both cases.
**Warning signs:** Alert never fires even when system is clearly idle.

### Pitfall 5: Dashboard JSON Schema Version Mismatch
**What goes wrong:** Dashboard loads but panels render incorrectly or with deprecation warnings.
**Why it happens:** Dashboard JSON `schemaVersion` doesn't match the Grafana version. Grafana 11.x uses schemaVersion ~39.
**How to avoid:** Export dashboards from the running Grafana instance (which uses the correct schema version) rather than writing JSON from scratch.
**Warning signs:** Console warnings about schema migration on dashboard load.

### Pitfall 6: Provisioned Alerting vs UI Alerting Conflict
**What goes wrong:** Alert rules appear duplicated or cannot be edited.
**Why it happens:** File-provisioned alert rules are read-only in the UI by default. If someone also creates alert rules via UI, there can be confusion about which are managed where.
**How to avoid:** All alert rules should be file-provisioned for this project. Do not create alert rules via the Grafana UI.
**Warning signs:** Duplicate alerts firing, inability to edit rules.

## Metrics Inventory for Dashboards

### Feed Health Dashboard (MON-04)

| Panel | Metric | Type | PromQL | Visualization |
|-------|--------|------|--------|---------------|
| Feed Availability | `feed_available` | gauge | `feed_available` | Stat (per venue) |
| Reconnection Rate | `feed_reconnections_total` | counter | `rate(feed_reconnections_total[5m])` | Time series (per venue) |
| Message Latency | `feed_last_latency_ms` | gauge | `feed_last_latency_ms` | Time series (per venue) |
| Latency Distribution | `feed_latency_ms` | histogram | `histogram_quantile(0.95, rate(feed_latency_ms_bucket[5m]))` | Time series |
| Message Rate | `feed_messages_total` | counter | `rate(feed_messages_total[1m])` | Time series (per venue) |
| Heartbeat Timeouts | `feed_heartbeat_timeouts` | counter | `increase(feed_heartbeat_timeouts[1h])` | Stat (per venue) |

### Signal Quality Dashboard (MON-05)

| Panel | Metric | Type | PromQL | Visualization |
|-------|--------|------|--------|---------------|
| Arb Signals Emitted | `arb_signals_emitted_total` | counter | `rate(arb_signals_emitted_total[5m])` | Time series |
| Signals Filtered | `arb_signals_filtered_total` | counter | `rate(arb_signals_filtered_total[5m])` | Time series |
| Staleness Rejections | `arb_staleness_rejections` | counter | `rate(arb_staleness_rejections[5m])` | Time series |
| Staleness Rejection Rate | computed | ratio | `rate(arb_staleness_rejections[5m]) / rate(arb_computations_total[5m])` | Gauge |
| Spread Signals | `spread_signals_total` | counter | `rate(spread_signals_total[5m])` | Time series (per event) |
| Events Tracked | `arb_events_tracked` | gauge | `arb_events_tracked` | Stat |
| Computation Rate | `arb_computations_total` | counter | `rate(arb_computations_total[5m])` | Time series |

### Paper Trade P&L Dashboard (MON-06)

| Panel | Metric | Type | PromQL | Visualization |
|-------|--------|------|--------|---------------|
| Daily P&L | `paper_trade_daily_pnl` | gauge | `paper_trade_daily_pnl` | Stat (big number) |
| Daily Win Rate | `paper_trade_daily_win_rate` | gauge | `paper_trade_daily_win_rate` | Gauge (0-1 range) |
| Daily Trades | `paper_trade_daily_trades` | gauge | `paper_trade_daily_trades` | Stat |
| Open Trades | `paper_trades_open` | gauge | `paper_trades_open` | Stat |
| Total Settled | `paper_trades_settled_total` | counter | `paper_trades_settled_total` | Stat (per outcome) |
| Settlement Timeouts | `settlement_timeouts_total` | counter | `increase(settlement_timeouts_total[1h])` | Stat (per venue) |
| Trade History | `paper_trades_total` | counter | `increase(paper_trades_total[1d])` | Time series |
| Cumulative Net P&L | `paper_trade_daily_pnl` | gauge | `paper_trade_daily_pnl` over time | Time series |

### System Health Dashboard (MON-07)

| Panel | Metric | Type | PromQL | Visualization |
|-------|--------|------|--------|---------------|
| Active Expiries | `pricing_active_expiries` | gauge | `pricing_active_expiries` | Stat |
| Active Subscriptions | `subscription_active` | gauge | `subscription_active` | Stat (per venue) |
| Lifecycle Polls | `lifecycle_discovery_polls` | counter | `rate(lifecycle_discovery_polls[5m])` | Time series (per venue) |
| Proposals Pending | `proposals_pending` | gauge | `proposals_pending` | Stat |
| Proposals Total | `proposals_total` | counter | `increase(proposals_total[1h])` | Time series |
| Active Alerts | `alert_monitor_active_alerts` | gauge | `alert_monitor_active_alerts` | Stat |
| Alert State | `alert_active` | gauge | `alert_active` | Table (per type) |
| Candidates Discovered | `lifecycle_candidates_discovered` | counter | `increase(lifecycle_candidates_discovered[1h])` | Stat |

### Alert Rules (MON-08)

| Alert | Metric | Condition | For Duration |
|-------|--------|-----------|--------------|
| Feed Down | `feed_available` | `< 1` (any venue) | 5m |
| Zero Spread Computations | `spread_computations_total` | `rate() == 0` | 30m |
| High Staleness Rejection Rate | `arb_staleness_rejections` / `arb_computations_total` | ratio > threshold (e.g., 0.5) | 5m |

## Code Examples

### Dashboard Provider YAML (Complete)

```yaml
# grafana/provisioning/dashboards/provider.yml
apiVersion: 1

providers:
  - name: prediction-dashboards
    orgId: 1
    type: file
    disableDeletion: false
    allowUiUpdates: true
    updateIntervalSeconds: 30
    options:
      path: /etc/grafana/provisioning/dashboards/json
```

### Data Source YAML Update (Add UID)

```yaml
# grafana/provisioning/datasources/amp.yml (updated)
apiVersion: 1

datasources:
  - name: AMP
    uid: amp
    type: prometheus
    access: proxy
    url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/ws-622e90d2-1edc-48ed-95dd-5d4938ca6659/
    isDefault: true
    jsonData:
      httpMethod: POST
      sigV4Auth: true
      sigV4AuthType: default
      sigV4Region: us-east-1
    editable: true
```

### Minimal Dashboard JSON Example (Feed Health)

```json
{
  "uid": "feed-health",
  "title": "Feed Health",
  "tags": ["prediction", "feeds"],
  "timezone": "utc",
  "editable": true,
  "graphTooltip": 1,
  "panels": [
    {
      "id": 1,
      "title": "Feed Availability",
      "type": "stat",
      "gridPos": { "h": 4, "w": 24, "x": 0, "y": 0 },
      "datasource": { "type": "prometheus", "uid": "amp" },
      "targets": [
        {
          "expr": "feed_available",
          "legendFormat": "{{venue}}",
          "refId": "A"
        }
      ],
      "fieldConfig": {
        "defaults": {
          "mappings": [
            { "type": "value", "options": { "0": { "text": "DOWN", "color": "red" }, "1": { "text": "UP", "color": "green" } } }
          ],
          "thresholds": {
            "mode": "absolute",
            "steps": [
              { "color": "red", "value": null },
              { "color": "green", "value": 1 }
            ]
          }
        },
        "overrides": []
      },
      "options": {
        "reduceOptions": { "calcs": ["lastNotNull"], "fields": "", "values": false },
        "orientation": "horizontal",
        "colorMode": "background"
      }
    },
    {
      "id": 2,
      "title": "Reconnection Rate (per venue)",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 12, "x": 0, "y": 4 },
      "datasource": { "type": "prometheus", "uid": "amp" },
      "targets": [
        {
          "expr": "rate(feed_reconnections_total[5m])",
          "legendFormat": "{{venue}}",
          "refId": "A"
        }
      ]
    },
    {
      "id": 3,
      "title": "Message Latency (ms)",
      "type": "timeseries",
      "gridPos": { "h": 8, "w": 12, "x": 12, "y": 4 },
      "datasource": { "type": "prometheus", "uid": "amp" },
      "targets": [
        {
          "expr": "feed_last_latency_ms",
          "legendFormat": "{{venue}}",
          "refId": "A"
        }
      ]
    }
  ],
  "time": { "from": "now-1h", "to": "now" },
  "refresh": "30s",
  "schemaVersion": 39,
  "version": 1
}
```

### Alert Rules YAML (Complete for MON-08)

```yaml
# grafana/provisioning/alerting/rules.yml
apiVersion: 1

groups:
  - orgId: 1
    name: prediction-critical
    folder: Prediction Alerts
    interval: 1m
    rules:
      # Alert 1: Feed down for 5 minutes
      - uid: feed-down
        title: Feed Down
        condition: B
        for: 5m
        noDataState: Alerting
        execErrState: Alerting
        data:
          - refId: A
            relativeTimeRange:
              from: 600
              to: 0
            datasourceUid: amp
            model:
              editorMode: code
              expr: feed_available
              instant: true
              intervalMs: 1000
              maxDataPoints: 43200
              refId: A
          - refId: B
            datasourceUid: __expr__
            model:
              conditions:
                - evaluator:
                    params:
                      - 1
                    type: lt
                  operator:
                    type: and
                  query:
                    params:
                      - B
                  reducer:
                    params: []
                    type: last
                  type: query
              datasource:
                type: __expr__
                uid: __expr__
              expression: A
              intervalMs: 1000
              maxDataPoints: 43200
              refId: B
              type: threshold
        annotations:
          summary: "Feed {{ $labels.venue }} is DOWN"
          description: "Feed {{ $labels.venue }} has been unavailable for more than 5 minutes"
        labels:
          severity: critical

      # Alert 2: Zero spread computations for 30 minutes
      - uid: zero-spread-computations
        title: Zero Spread Computations
        condition: B
        for: 30m
        noDataState: Alerting
        execErrState: Alerting
        data:
          - refId: A
            relativeTimeRange:
              from: 1800
              to: 0
            datasourceUid: amp
            model:
              editorMode: code
              expr: rate(spread_computations_total[5m])
              instant: true
              intervalMs: 1000
              maxDataPoints: 43200
              refId: A
          - refId: B
            datasourceUid: __expr__
            model:
              conditions:
                - evaluator:
                    params:
                      - 0
                    type: lt
                  operator:
                    type: and
                  query:
                    params:
                      - B
                  reducer:
                    params: []
                    type: last
                  type: query
              datasource:
                type: __expr__
                uid: __expr__
              expression: A
              intervalMs: 1000
              maxDataPoints: 43200
              refId: B
              type: threshold
        annotations:
          summary: "No spread computations for 30 minutes"
          description: "The system has not performed any spread computations in the last 30 minutes"
        labels:
          severity: warning

      # Alert 3: High staleness rejection rate
      - uid: high-staleness-rate
        title: High Staleness Rejection Rate
        condition: B
        for: 5m
        noDataState: OK
        execErrState: Alerting
        data:
          - refId: A
            relativeTimeRange:
              from: 600
              to: 0
            datasourceUid: amp
            model:
              editorMode: code
              expr: rate(arb_staleness_rejections[5m]) / rate(arb_computations_total[5m])
              instant: true
              intervalMs: 1000
              maxDataPoints: 43200
              refId: A
          - refId: B
            datasourceUid: __expr__
            model:
              conditions:
                - evaluator:
                    params:
                      - 0.5
                    type: gt
                  operator:
                    type: and
                  query:
                    params:
                      - B
                  reducer:
                    params: []
                    type: last
                  type: query
              datasource:
                type: __expr__
                uid: __expr__
              expression: A
              intervalMs: 1000
              maxDataPoints: 43200
              refId: B
              type: threshold
        annotations:
          summary: "Staleness rejection rate exceeds 50%"
          description: "More than 50% of arb computations are being rejected due to stale data"
        labels:
          severity: warning
```

### Contact Points YAML (Minimal -- Grafana Built-in)

```yaml
# grafana/provisioning/alerting/contact-points.yml
apiVersion: 1

contactPoints:
  - orgId: 1
    name: grafana-default-email
    receivers:
      - uid: grafana-default-email
        type: email
        disableResolveMessage: false
        settings:
          addresses: ""
          singleEmail: false
```

Note: For a single-developer project, the default Grafana alerting UI (visible in Grafana at Alerting > Alert rules) is sufficient. Alerts show state changes in the UI without requiring external notification channels. Email/webhook contact points can be configured later if needed.

### Notification Policies YAML (Minimal)

```yaml
# grafana/provisioning/alerting/notification-policies.yml
apiVersion: 1

policies:
  - orgId: 1
    receiver: grafana-default-email
    group_by:
      - grafana_folder
      - alertname
```

## CDK User-Data Integration

The CDK user-data in `prediction-stack.ts` must be updated to write all new provisioning files before starting docker-compose. The pattern follows the existing approach used for `amp.yml`:

1. Create directories: `mkdir -p /opt/prediction/grafana/provisioning/{dashboards/json,alerting}`
2. Write `provider.yml` (dashboard provider config)
3. Write each dashboard JSON file
4. Write `rules.yml`, `contact-points.yml`, `notification-policies.yml`
5. Update existing `amp.yml` write to include `uid: amp`

All files must be written BEFORE the docker-compose.yml heredoc and service start.

## Recommended Implementation Strategy

**Build-then-export approach (recommended):**

1. First deploy with minimal/skeleton dashboard JSON files and alert rules via provisioning
2. Open Grafana UI and refine dashboards interactively (allowed by `allowUiUpdates: true`)
3. Export the refined dashboard JSON from Grafana UI (Dashboard Settings > JSON Model)
4. Replace the skeleton JSON files with the exported versions
5. Commit the final JSON files to the repo
6. Update CDK user-data with final files

This is more practical than writing pixel-perfect dashboard JSON by hand. The provisioning system ensures reproducibility; the UI ensures usability.

**Alternative: fully code-driven approach:**

Write complete dashboard JSON files directly. This works but is tedious for panel positioning and field configuration. Best used when dashboards are simple (stats + time series only).

Given the dashboards are relatively straightforward (stats, time series, gauges), the code-driven approach is feasible for this project. The planner should decide based on complexity.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Legacy alerting (Dashboard alerts) | Unified Alerting (Grafana 9+) | Grafana 9.0 (2022) | Alert rules are separate from dashboards; file provisioning for alerts |
| Grafana API keys | Service account tokens | Grafana 9.x+ | API keys deprecated |
| Angular panels | React panels | Grafana 10+ | Old angular panel types deprecated |
| `graph` panel type | `timeseries` panel type | Grafana 8+ | Use `timeseries` not `graph` for time series charts |

**Deprecated/outdated:**
- `graph` panel type: replaced by `timeseries`
- Dashboard-embedded alerts: replaced by unified alerting rules
- API keys: replaced by service account tokens

## Open Questions

1. **Staleness rejection rate threshold**
   - What we know: Alert should fire when staleness rejection rate exceeds a threshold
   - What's unclear: The exact threshold value (50%? 80%? configurable?)
   - Recommendation: Start with 50% (0.5 ratio) as a reasonable default. Can be adjusted in the provisioning YAML without code changes.

2. **Net edge BPS and confidence metrics**
   - What we know: MON-05 requirements list "net_edge_bps" and "confidence" but these are not in the metrics inventory
   - What's unclear: Whether these should be added as application metrics or if existing signal metrics are sufficient
   - Recommendation: Use available signal metrics (`arb_signals_emitted_total`, `arb_signals_filtered_total`, `arb_staleness_rejections`). The signal analysis metrics (`signal_analysis_filtered_hypothetical_hit_rate`, `signal_analysis_daily_net_hit_rate`) may serve as proxies. Note this gap for the planner.

3. **External notification delivery**
   - What we know: Single-developer project; alerts visible in Grafana UI
   - What's unclear: Whether email/webhook/SMS notifications are needed beyond UI visibility
   - Recommendation: Provision a minimal contact point structure. Email requires SMTP configuration on the Grafana container. For now, Grafana UI alert state visibility is sufficient.

## Sources

### Primary (HIGH confidence)
- [Grafana Provisioning Documentation](https://grafana.com/docs/grafana/latest/administration/provisioning/) - Dashboard and data source provisioning format
- [Grafana Alerting File Provisioning](https://grafana.com/docs/grafana/latest/alerting/set-up/provision-alerting-resources/file-provisioning/) - Alert rule YAML format
- [Grafana Dashboard JSON Model](https://grafana.com/docs/grafana/latest/visualizations/dashboards/build-dashboards/view-dashboard-json-model/) - Dashboard JSON structure
- Phase 37 Research and Summary - Existing infrastructure details, metrics inventory
- Codebase: `grafana/provisioning/datasources/amp.yml` - Current provisioning setup
- Codebase: `docker-compose.yml` - Current Grafana container configuration
- Codebase: `infra/cdk/lib/prediction-stack.ts` - User-data provisioning pattern

### Secondary (MEDIUM confidence)
- [Grafana provisioning-alerting-examples](https://github.com/grafana/provisioning-alerting-examples) - Official example repo for alerting provisioning
- [Grafana Dashboard Provisioning Tutorial](https://grafana.com/tutorials/provision-dashboards-and-data-sources/) - Dashboard provider YAML structure
- [Grafana Community: Provisioned dashboard datasource UIDs](https://community.grafana.com/t/should-provisioned-dashboards-have-datasource-uids/65463) - Best practices for UID references

### Tertiary (LOW confidence)
- Alert rule YAML examples synthesized from multiple sources; exact field behavior should be verified on the running Grafana instance
- Dashboard JSON panel field config options are version-dependent; export from running instance is the authoritative source

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Using already-deployed Grafana OSS 11.5.2 with existing provisioning infrastructure
- Architecture: HIGH - File-based provisioning is well-documented and already used for data sources
- Dashboard metrics mapping: HIGH - Full metrics inventory verified in Phase 37 research
- Alert rule format: MEDIUM - YAML structure verified from official docs but exact field behavior needs runtime validation
- Pitfalls: HIGH - Data source UID issue is a known common problem; provisioning patterns are well-established

**Research date:** 2026-03-08
**Valid until:** 2026-04-08 (Grafana provisioning is a stable feature unlikely to change)
