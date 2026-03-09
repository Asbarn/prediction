# Phase 37: Prometheus + AMP + Managed Grafana - Research

**Researched:** 2026-03-07
**Domain:** AWS Managed Prometheus (AMP), Amazon Managed Grafana (AMG), Prometheus sidecar
**Confidence:** HIGH

## Summary

This phase provisions two AWS managed services (AMP workspace + Managed Grafana workspace) via CDK and adds a Prometheus sidecar container to docker-compose that scrapes the application's existing `:9000/metrics` endpoint and remote-writes to AMP using SigV4 authentication. The application already exposes 80+ Prometheus metrics via `metrics-exporter-prometheus` on port 9000 (health endpoint is on port 9001). The EC2 instance role already has `AmazonPrometheusRemoteWriteAccess` attached (added in Phase 34).

The key architectural choice is using Prometheus 3.x with native SigV4 support (available since Prometheus 2.26.0) rather than a separate SigV4 proxy sidecar. This eliminates an extra container and simplifies the docker-compose configuration. The Prometheus container runs alongside the prediction container, scrapes `localhost:9000/metrics`, and remote-writes to the AMP endpoint. Amazon Managed Grafana connects to AMP as a data source for visualization.

**Primary recommendation:** Add Prometheus 3.x sidecar to docker-compose with native `sigv4` remote_write, provision AMP + AMG workspaces in CDK, and connect AMG to AMP as a data source.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MON-02 | Prometheus sidecar scrapes :9001/metrics and remote_writes to Amazon Managed Prometheus with SigV4 auth | Prometheus 3.x native SigV4, docker-compose sidecar pattern, AMP CfnWorkspace CDK construct. NOTE: actual metrics endpoint is :9000, not :9001 (9001 is health). The requirement text says :9001 but the scrape target must be :9000. |
| MON-03 | Amazon Managed Grafana workspace connects to AMP as data source | AMG CfnWorkspace CDK construct with `dataSources: ['PROMETHEUS']`, IAM Identity Center or SAML auth required |
</phase_requirements>

## Critical Clarification: Port Mapping

The REQUIREMENTS.md says MON-02 scrapes `:9001/metrics`. However, in the actual codebase:
- **Port 9000**: Prometheus metrics exporter (`metrics-exporter-prometheus` via `setup_prometheus(port)`)
- **Port 9001**: Health/status endpoint (axum `GET /health`)

The Prometheus sidecar MUST scrape port **9000**, not 9001. The docker-compose already exposes both ports. The requirement text appears to have a typo -- the intent (scraping application Prometheus metrics) is clear.

## Standard Stack

### Core

| Component | Version/Service | Purpose | Why Standard |
|-----------|----------------|---------|--------------|
| prom/prometheus | v3.10.0 (latest stable) | Scrape app metrics, remote_write to AMP | Native SigV4 support eliminates proxy sidecar; standard Prometheus ecosystem |
| Amazon Managed Prometheus (AMP) | Managed service | Durable metrics storage + PromQL query | AWS-managed, scales automatically, survives EC2 lifecycle |
| Amazon Managed Grafana (AMG) | Managed service (Grafana 10.x) | Visualization and dashboarding | AWS-managed, native AMP data source integration |
| aws-cdk-lib/aws-aps | CfnWorkspace | CDK construct for AMP workspace | Only CDK construct available (L1, no L2 exists) |
| aws-cdk-lib/aws-grafana | CfnWorkspace | CDK construct for AMG workspace | Only CDK construct available (L1, no L2 exists) |

### Supporting

| Component | Purpose | When to Use |
|-----------|---------|-------------|
| AWS IAM Identity Center (SSO) | AMG authentication | Required for AMG -- must be enabled before workspace creation |
| AmazonPrometheusRemoteWriteAccess | IAM managed policy | Already attached to EC2 instance role (Phase 34) |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Native SigV4 in Prometheus | AWS SigV4 proxy sidecar (`aws-sigv4-proxy`) | Extra container, more complexity; native SigV4 is simpler |
| Amazon Managed Grafana | Self-hosted Grafana on EC2 | Explicitly out of scope per REQUIREMENTS.md |
| Prometheus sidecar | AWS Distro for OpenTelemetry (ADOT) | Heavier, designed for distributed tracing; Prometheus is simpler for pure metrics |

## Architecture Patterns

### Docker Compose Sidecar Pattern

The Prometheus sidecar runs as a second service in docker-compose, sharing the host network with the prediction container via port mapping. The sidecar scrapes `prediction:9000/metrics` (using docker-compose DNS) and remote-writes to AMP.

```yaml
services:
  prediction:
    # ... existing service unchanged ...
    ports:
      - "9000:9000"
      - "9001:9001"

  prometheus:
    image: prom/prometheus:v3.10.0
    volumes:
      - /opt/prediction/prometheus.yml:/etc/prometheus/prometheus.yml:ro
    restart: "no"
    depends_on:
      prediction:
        condition: service_healthy
```

### Prometheus Configuration (prometheus.yml)

```yaml
global:
  scrape_interval: 15s
  external_labels:
    instance: prediction-prod

scrape_configs:
  - job_name: prediction
    static_configs:
      - targets: ['prediction:9000']
    metrics_path: /metrics

remote_write:
  - url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/WORKSPACE_ID/api/v1/remote_write
    queue_config:
      max_samples_per_send: 1000
      max_shards: 200
      capacity: 2500
    sigv4:
      region: us-east-1
```

### CDK Infrastructure Pattern

```typescript
import * as aps from 'aws-cdk-lib/aws-aps';
import * as grafana from 'aws-cdk-lib/aws-grafana';

// AMP Workspace
const ampWorkspace = new aps.CfnWorkspace(this, 'AmpWorkspace', {
  alias: 'prediction-metrics',
  tags: [{ key: 'Project', value: 'prediction' }],
});

// AMG Workspace
const grafanaWorkspace = new grafana.CfnWorkspace(this, 'GrafanaWorkspace', {
  accountAccessType: 'ACCOUNT',
  authenticationProviders: ['AWS_SSO'],
  permissionType: 'SERVICE_MANAGED',
  name: 'prediction-dashboards',
  dataSources: ['PROMETHEUS'],
  roleArn: grafanaRole.roleArn, // if CUSTOMER_MANAGED
});

// Outputs
new cdk.CfnOutput(this, 'AmpWorkspaceId', {
  value: ampWorkspace.attrWorkspaceId,
});
new cdk.CfnOutput(this, 'AmpEndpoint', {
  value: ampWorkspace.attrPrometheusEndpoint,
});
new cdk.CfnOutput(this, 'GrafanaEndpoint', {
  value: grafanaWorkspace.attrEndpoint,
});
```

### Key CDK Outputs Needed

| Output | Source | Used By |
|--------|--------|---------|
| `AmpWorkspaceId` | `ampWorkspace.attrWorkspaceId` | prometheus.yml remote_write URL |
| `AmpEndpoint` | `ampWorkspace.attrPrometheusEndpoint` | Grafana data source configuration |
| `GrafanaEndpoint` | `grafanaWorkspace.attrEndpoint` | User access URL |

### Anti-Patterns to Avoid

- **Running Prometheus with persistent storage on EC2**: The sidecar should be stateless. AMP is the durable store. No need for Prometheus TSDB retention.
- **Scraping port 9001**: That is the health endpoint, not metrics. Must scrape port 9000.
- **Using `network_mode: host`**: Use docker-compose service DNS (`prediction:9000`) instead; host networking breaks container isolation.
- **Hardcoding workspace ID in prometheus.yml**: Template the config or write it via user-data after CDK outputs are available.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SigV4 request signing | Custom signing logic | Prometheus native `sigv4:` config | Built into Prometheus since 2.26; handles credential refresh from EC2 instance metadata |
| Metrics storage | Local Prometheus TSDB | Amazon Managed Prometheus | Survives EC2 lifecycle, scales, managed retention |
| Grafana hosting | Docker Grafana on EC2 | Amazon Managed Grafana | Explicitly out of scope; managed service handles patching, scaling, auth |
| IAM credential injection | Mounting AWS credentials | EC2 instance metadata (IMDSv2) | Prometheus SigV4 auto-discovers credentials from instance metadata |

**Key insight:** The entire value of this phase is eliminating self-managed monitoring. Every component should be managed or stateless.

## Common Pitfalls

### Pitfall 1: IAM Identity Center Not Enabled
**What goes wrong:** AMG requires either AWS_SSO or SAML authentication. CDK deploy fails if Identity Center is not set up.
**Why it happens:** Single-developer accounts often don't have Identity Center enabled.
**How to avoid:** Enable IAM Identity Center in the AWS console before CDK deploy. Create at least one SSO user. This is a one-time manual step.
**Warning signs:** CDK deploy error mentioning `authenticationProviders` or SSO configuration.

### Pitfall 2: Prometheus Cannot Reach AMP Endpoint
**What goes wrong:** SigV4 signing fails or network timeout on remote_write.
**Why it happens:** EC2 instance needs outbound HTTPS access to `aps-workspaces.us-east-1.amazonaws.com`. Security group must allow all outbound (already configured). Instance must have public IP or NAT (public subnet, already configured).
**How to avoid:** Verify outbound connectivity. The current security group has `allowAllOutbound: true` which is correct.
**Warning signs:** Prometheus logs showing `remote_write` errors, 403 or timeout responses.

### Pitfall 3: Workspace ID Not Available at Docker-Compose Time
**What goes wrong:** prometheus.yml needs the AMP workspace ID, but it is only known after CDK deploy.
**Why it happens:** CDK creates the workspace, then the EC2 instance needs the workspace ID in its prometheus.yml.
**How to avoid:** Write the workspace ID into user-data via CDK output substitution, or store it in SSM Parameter Store and retrieve it at boot time in the fetch-secrets.sh script.
**Warning signs:** Placeholder workspace ID in prometheus.yml, metrics not flowing.

### Pitfall 4: Prometheus Scrape Target Unreachable
**What goes wrong:** Prometheus cannot reach the prediction container's metrics endpoint.
**Why it happens:** Docker-compose service names are only resolvable within the same compose network. If using `localhost` instead of the service name, the port must be exposed.
**How to avoid:** Use `prediction:9000` as the scrape target (docker-compose service DNS). The `depends_on` with `service_healthy` ensures the prediction container is up first.
**Warning signs:** Prometheus targets page showing the target as DOWN.

### Pitfall 5: Grafana Data Source Not Connected to AMP
**What goes wrong:** Grafana workspace exists but cannot query metrics.
**Why it happens:** The AMP data source must be added in the Grafana UI or via API after workspace creation. CDK `dataSources: ['PROMETHEUS']` only grants IAM permission -- it does not configure the actual data source in Grafana.
**How to avoid:** After CDK deploy, manually add the AMP data source in the Grafana console, or use the Grafana HTTP API with a service account token.
**Warning signs:** Empty query results in Grafana despite metrics flowing to AMP.

### Pitfall 6: Prometheus 3.x Configuration Changes
**What goes wrong:** Config syntax differences between Prometheus 2.x docs and Prometheus 3.x actual behavior.
**Why it happens:** Most AMP documentation references Prometheus 2.x. Prometheus 3.0 was released November 2024 and has some breaking changes.
**How to avoid:** Pin to `prom/prometheus:v3.10.0` and verify config with `promtool check config prometheus.yml`.
**Warning signs:** Prometheus container crashes on startup with config parse errors.

## Code Examples

### prometheus.yml (Complete)

```yaml
# Source: AWS docs + Prometheus 3.x docs
global:
  scrape_interval: 15s
  evaluation_interval: 15s
  external_labels:
    cluster: prediction-prod
    instance: ec2

scrape_configs:
  - job_name: prediction
    static_configs:
      - targets: ['prediction:9000']
    scrape_interval: 15s
    metrics_path: /metrics

remote_write:
  - url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/${AMP_WORKSPACE_ID}/api/v1/remote_write
    queue_config:
      max_samples_per_send: 1000
      max_shards: 200
      capacity: 2500
    sigv4:
      region: us-east-1
```

### Docker Compose Addition

```yaml
  prometheus:
    image: prom/prometheus:v3.10.0
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.retention.time=2h'
      - '--web.enable-lifecycle'
    volumes:
      - /opt/prediction/prometheus.yml:/etc/prometheus/prometheus.yml:ro
    depends_on:
      prediction:
        condition: service_healthy
    restart: "no"
```

Note: `--storage.tsdb.retention.time=2h` keeps local TSDB minimal since AMP is the durable store.

### CDK AMP Workspace

```typescript
// Source: AWS CDK docs aws-cdk-lib/aws-aps
const ampWorkspace = new aps.CfnWorkspace(this, 'AmpWorkspace', {
  alias: 'prediction-metrics',
});

new cdk.CfnOutput(this, 'AmpWorkspaceId', {
  value: ampWorkspace.attrWorkspaceId,
});
new cdk.CfnOutput(this, 'AmpPrometheusEndpoint', {
  value: ampWorkspace.attrPrometheusEndpoint,
});
```

### CDK AMG Workspace

```typescript
// Source: AWS CDK docs aws-cdk-lib/aws-grafana
const grafanaRole = new iam.Role(this, 'GrafanaRole', {
  assumedBy: new iam.ServicePrincipal('grafana.amazonaws.com'),
  description: 'Amazon Managed Grafana workspace role',
});

// Grant Grafana read access to AMP
grafanaRole.addToPolicy(new iam.PolicyStatement({
  effect: iam.Effect.ALLOW,
  actions: [
    'aps:QueryMetrics',
    'aps:GetSeries',
    'aps:GetLabels',
    'aps:GetMetricMetadata',
  ],
  resources: [ampWorkspace.attrArn],
}));

const grafanaWorkspace = new grafana.CfnWorkspace(this, 'GrafanaWorkspace', {
  accountAccessType: 'ACCOUNT',
  authenticationProviders: ['AWS_SSO'],
  permissionType: 'CUSTOMER_MANAGED',
  name: 'prediction-dashboards',
  dataSources: ['PROMETHEUS'],
  roleArn: grafanaRole.roleArn,
});

new cdk.CfnOutput(this, 'GrafanaEndpoint', {
  value: grafanaWorkspace.attrEndpoint,
});
new cdk.CfnOutput(this, 'GrafanaWorkspaceId', {
  value: grafanaWorkspace.attrId,
});
```

### User-Data: Write Prometheus Config with Workspace ID

```bash
# After CDK deploy, the workspace ID is known. Write prometheus.yml:
AMP_WORKSPACE_ID="<from-cdk-output>"

cat > /opt/prediction/prometheus.yml <<PROMEOF
global:
  scrape_interval: 15s
  external_labels:
    cluster: prediction-prod

scrape_configs:
  - job_name: prediction
    static_configs:
      - targets: ['prediction:9000']

remote_write:
  - url: https://aps-workspaces.us-east-1.amazonaws.com/workspaces/${AMP_WORKSPACE_ID}/api/v1/remote_write
    queue_config:
      max_samples_per_send: 1000
      max_shards: 200
      capacity: 2500
    sigv4:
      region: us-east-1
PROMEOF
```

## Application Metrics Inventory

The prediction application exposes the following metric families (80+ individual time series with labels):

### Feed Metrics
- `feed_available` (gauge, per venue: deribit, polymarket, kalshi, derive)
- `feed_messages_total` (counter, per venue)
- `feed_last_latency_ms` (gauge, per venue)
- `feed_reconnections_total` (counter, per venue)
- `feed_heartbeat_timeouts` (counter, per venue)
- `feed_latency_ms` (histogram, custom ms buckets)

### Spread/Signal Metrics
- `spread_computations_total` (counter, per event)
- `spread_signals_total` (counter, per event + pattern)
- `spread_staleness_rejections` (counter, per event + venue)
- `spread_rolling_mean` (gauge, per event)
- `spread_rolling_stddev` (gauge, per event)
- `arb_signals_emitted_total` (counter)
- `arb_signals_filtered_total` (counter)
- `arb_computations_total` (counter)
- `arb_staleness_rejections` (counter)
- `arb_events_tracked` (gauge)
- `arb_unmapped_instruments_total` (counter)

### Paper Trading Metrics
- `paper_trade_signals_total` (counter)
- `paper_trades_total` (counter, per event)
- `paper_trades_open` (gauge)
- `paper_trades_settled_total` (counter, per event + outcome)
- `paper_trade_divergence_total` (counter)
- `paper_trade_daily_pnl` (gauge)
- `paper_trade_daily_trades` (gauge)
- `paper_trade_daily_win_rate` (gauge)
- `persistence_checkpoints_written` (counter)

### Lifecycle Metrics
- `lifecycle_discovery_polls` (counter, per venue)
- `lifecycle_candidates_discovered` (counter)
- `lifecycle_events_archived` (counter)
- `lifecycle_candidates_cleaned` (counter)
- `lifecycle_expiry_warnings` (gauge)
- `proposals_total` (counter)
- `proposals_pending` (gauge)

### Pricing Metrics
- `pricing_iv_solves_total` (counter)
- `pricing_active_expiries` (gauge)

### Subscription Metrics
- `subscription_active` (gauge, per venue)
- `subscription_activations_total` (counter, per venue)
- `subscription_removals_total` (counter, per venue)

### Settlement Metrics
- `settlement_timeouts_total` (counter, per venue)
- `settlement_outcomes_total` (counter, per venue)

### Alert Metrics
- `alert_monitor_active_alerts` (gauge)
- `alert_active` (gauge, per type)

### Signal Analysis Metrics
- `signal_analysis_filtered_hypothetical_hit_rate` (gauge)
- `signal_analysis_daily_settled` (gauge)
- `signal_analysis_daily_net_hit_rate` (gauge)
- Various per-type analysis gauges

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SigV4 proxy sidecar container | Prometheus native SigV4 (2.26+) | 2021 (Prometheus 2.26) | Eliminates extra container |
| Prometheus 2.x | Prometheus 3.x (3.10.0) | Nov 2024 | New config features, breaking changes in some areas |
| Grafana API keys | Service account tokens | Grafana 9.x+ | API keys deprecated, service accounts preferred |
| AMG free users | $9/editor, $5/viewer per month | Current pricing | 90-day free trial available |

## Cost Considerations

| Service | Cost | Notes |
|---------|------|-------|
| AMP ingestion | $0.90 per million samples | ~80 metrics * 4 scrapes/min * 43800 min/month = ~14M samples/month = ~$12.60/month |
| AMP storage | $0.03 per GB-month | Minimal for this volume |
| AMG workspace | $9/month minimum | 1 editor license minimum; 90-day free trial |
| **Total estimated** | **~$22/month** | After free trial |

## Deployment Order

1. **Enable IAM Identity Center** (manual, one-time, prerequisite for AMG)
2. **CDK deploy** -- adds AMP workspace + AMG workspace + IAM role for Grafana
3. **Note AMP workspace ID** from CDK outputs
4. **Update user-data** to write prometheus.yml with workspace ID and add prometheus service to docker-compose
5. **Restart/redeploy EC2** to pick up new docker-compose with Prometheus sidecar
6. **Configure AMG data source** -- add AMP as Prometheus data source in Grafana UI
7. **Verify** -- query metrics in Grafana, confirm persistence across restart

## Open Questions

1. **Workspace ID injection strategy**
   - What we know: prometheus.yml needs the AMP workspace ID; CDK creates it dynamically
   - What's unclear: Best way to pass workspace ID to EC2 user-data (SSM Parameter Store vs CDK output in user-data vs hardcode after first deploy)
   - Recommendation: Store workspace ID in SSM Parameter Store via CDK, retrieve in fetch-secrets.sh alongside credentials. Alternatively, since this is a single-developer project, hardcoding after first CDK deploy is pragmatic.

2. **AMG data source configuration automation**
   - What we know: CDK `dataSources: ['PROMETHEUS']` only sets IAM permissions, not the actual Grafana data source
   - What's unclear: Whether to automate via Grafana API or just configure manually in UI
   - Recommendation: Manual configuration via Grafana UI is fine for a single-developer project. Document the steps.

3. **Prometheus 3.x compatibility with AMP**
   - What we know: AWS docs reference Prometheus 2.x; Prometheus 3.10.0 is current
   - What's unclear: Any breaking changes in 3.x affecting remote_write or SigV4
   - Recommendation: The remote_write and sigv4 config format is unchanged. Pin to v3.10.0 and verify with promtool.

## Sources

### Primary (HIGH confidence)
- [AWS CDK CfnWorkspace (APS)](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_aps.CfnWorkspace.html) - AMP CDK construct API
- [AWS CDK CfnWorkspace (Grafana)](https://docs.aws.amazon.com/cdk/api/v2/docs/aws-cdk-lib.aws_grafana.CfnWorkspace.html) - AMG CDK construct API
- [AMP EC2 Remote Write Setup](https://docs.aws.amazon.com/prometheus/latest/userguide/AMP-onboard-ingest-metrics-remote-write-EC2.html) - Official EC2 ingestion guide
- [Prometheus 3.10.0 Release](https://github.com/prometheus/prometheus/releases/tag/v3.10.0) - Current stable version
- Codebase: `src/metrics_export/mod.rs` - Prometheus endpoint on port 9000
- Codebase: `src/health/mod.rs` - Health endpoint on port 9001
- Codebase: `infra/cdk/lib/prediction-stack.ts` - IAM role already has AMP write access

### Secondary (MEDIUM confidence)
- [Amazon Managed Grafana Pricing](https://aws.amazon.com/grafana/pricing/) - $9/editor/month
- [Prometheus SigV4 Support Blog](https://aws.amazon.com/blogs/opensource/prometheus-2-26-0-adds-aws-signature-version-4-support/) - Native SigV4 since 2.26
- [AMG Authentication](https://docs.aws.amazon.com/grafana/latest/userguide/authentication-in-AMG.html) - SSO or SAML required

### Tertiary (LOW confidence)
- AMP cost estimate based on sample rate calculations (actual may vary with label cardinality)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - CDK constructs verified in official docs, Prometheus SigV4 well-documented
- Architecture: HIGH - Docker-compose sidecar pattern is straightforward, codebase ports verified
- Pitfalls: HIGH - SSO requirement, port confusion, workspace ID injection are well-understood issues

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable managed services, unlikely to change)
