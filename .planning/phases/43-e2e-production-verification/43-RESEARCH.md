# Phase 43: End-to-End Production Verification - Research

**Researched:** 2026-03-09
**Domain:** Production deployment verification, AWS SSM operations, Grafana observability, JSONL log inspection
**Confidence:** HIGH

## Summary

Phase 43 is a verification-only phase -- no new code features are needed. The goal is to deploy the completed v1.7 code (Phases 40-42) to production EC2 and prove that cross-asset arbitrage signals are being generated from live market data. Verification requires two observable outcomes: (1) Grafana dashboards show signal activity, and (2) JSONL log files contain real ArbSignal and SpreadResult entries.

There is one critical infrastructure gap discovered during research: the `signal_logs` directory is NOT mounted as a Docker volume in either the local `docker-compose.yml` or the CDK-generated docker-compose on EC2. Signal JSONL logs are written to `/app/signal_logs` inside the container and will be lost on container restart. This must be fixed before verification can succeed for VER-02. The `spread_logs` directory IS properly mounted.

**Primary recommendation:** Fix the signal_logs volume mount gap (both docker-compose.yml and CDK user-data), deploy via GitLab CI/CD, then verify signals appear in Grafana metrics and JSONL log files on the EC2 host.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| VER-01 | Production system generates cross-asset arbitrage signals visible in Grafana dashboards | Grafana signal-quality dashboard already has panels for `arb_signals_emitted_total`, `arb_computations_total`, `arb_events_tracked`. Metrics are emitted by CrossAssetEngine. Verification = non-zero values in these panels. |
| VER-02 | Signal and spread JSONL logs contain entries from live production data | SignalLogger writes to `signal_logs/{YYYY-MM-DD}.jsonl`, SpreadLogger writes to `spread_logs/{YYYY-MM-DD}.jsonl`. **Critical gap:** signal_logs volume mount is missing from docker-compose.yml and CDK. Must be added before deploy. |
</phase_requirements>

## Architecture Patterns

### Deployment Flow

The deployment path is well-established from Phase 38:

1. Push to `master` branch triggers GitLab CI/CD
2. Pipeline: `test` -> `build-and-push` (Docker to ECR) -> `deploy` (SSM send-command)
3. Deploy stage: `systemctl stop prediction` -> `fetch-secrets.sh` -> `docker compose pull` -> `systemctl start prediction` -> health check
4. Health check: `curl -sf http://localhost:9001/health` with 5 retries

### SSM Command Execution from Local Machine

For ad-hoc verification commands on EC2, use AWS SSM:

```bash
# CRITICAL: On Windows/Git Bash, prefix with MSYS_NO_PATHCONV=1
MSYS_NO_PATHCONV=1 aws ssm send-command \
  --instance-ids "$EC2_INSTANCE_ID" \
  --document-name "AWS-RunShellScript" \
  --parameters 'commands=["docker exec prediction-prediction-1 ls /app/signal_logs/"]' \
  --query "Command.CommandId" \
  --output text
```

### Docker Volume Mounts (Current State)

```
/opt/prediction/data/config         -> /app/config
/opt/prediction/data/spread_logs    -> /app/spread_logs       (EXISTS)
/opt/prediction/data/settlement_logs -> /app/settlement_logs
/opt/prediction/data/paper_trades   -> /app/paper_trades
/opt/prediction/data/state          -> /app/state
/opt/prediction/data/logs           -> /app/logs
# MISSING: signal_logs mount!
```

### Signal Pipeline Data Flow

```
Deribit/Derive WS -> ImpliedProbability (with source_venue)
                        |
Polymarket WS/REST -> MarketSnapshot
                        |
                   CrossAssetEngine
                        |
                    ArbSignal
                   /         \
        SignalLogger        Prometheus metrics
     (signal_logs/*.jsonl)  (arb_signals_emitted_total, etc.)
                               |
                            AMP scrape
                               |
                         Grafana dashboards
                         (signal-quality)
```

### Grafana Dashboard Panels for Signal Verification

The `signal-quality` dashboard (uid: `signal-quality`) already contains:

| Panel | Metric | What to Look For |
|-------|--------|-----------------|
| Events Tracked | `arb_events_tracked` | Non-zero gauge = events are mapped and tracked |
| Computation Rate | `rate(arb_computations_total[5m])` | Non-zero = engine is computing signals |
| Arb Signals Emitted | `rate(arb_signals_emitted_total[5m])` | Non-zero = signals pass threshold |
| Signals Filtered | `rate(arb_signals_filtered_total[5m])` | Expected to be non-zero (cooldown filtering) |
| Staleness Rejections | `rate(arb_staleness_rejections[5m])` | Expected non-zero initially; should decrease as feeds stabilize |

The `feed-health` dashboard (uid: `feed-health`) shows:

| Panel | Metric | What to Look For |
|-------|--------|-----------------|
| Feed Availability | `feed_available{venue}` | Polymarket + Deribit/Derive should be UP (1) |
| Feed Messages | `feed_messages_total{venue}` | All venues should show message flow |

### JSONL Log File Structure

Signal logs (`signal_logs/YYYY-MM-DD.jsonl`): Each line is a serialized `ArbSignal` with fields including `event_id`, `direction`, `raw_spread`, `net_edge`, `prediction_leg` (with venue), `options_leg` (with venue), `prediction_venue`, `timestamp`.

Spread logs (`spread_logs/YYYY-MM-DD.jsonl`): Each line is a serialized `SpreadResult` with fields including `event_id`, `pattern`, `gross_spread`, `net_spread`, `timestamp_ms`, `poly_exchange_ts`.

### Config Defaults

Signal log directory defaults to `signal_logs` in `SignalGenerationConfig` (src/signal/config.rs:84). Not overridden in config/config.toml. Spread log directory is explicitly set to `spread_logs` in config/config.toml:20.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| EC2 command execution | SSH tunnel | AWS SSM send-command | Already established pattern; no SSH keys needed |
| Log file inspection | Custom scripts | `docker exec` + `cat`/`wc -l` via SSM | Container filesystem access through existing tooling |
| Grafana verification | Screenshot comparison | Check metric values via AMP query or Grafana API | Objective, automatable |
| Deployment | Manual docker pull | GitLab CI/CD pipeline push to master | Established in Phase 38 |

## Common Pitfalls

### Pitfall 1: Missing signal_logs Volume Mount
**What goes wrong:** Signal JSONL files are written inside the Docker container at `/app/signal_logs` and are lost on container restart or redeploy
**Why it happens:** The volume mount was added for `spread_logs` but never added for `signal_logs` (signal engine was added in Phase 8, volume mounts were set up in Phase 34/35)
**How to avoid:** Add `signal_logs` volume mount to BOTH local `docker-compose.yml` AND CDK user-data `prediction-stack.ts`. Also add `mkdir -p` for the data directory.
**Warning signs:** Empty `signal_logs` directory on host, or directory not existing at all

### Pitfall 2: MSYS_NO_PATHCONV on Windows/Git Bash
**What goes wrong:** Git Bash on Windows converts `/opt/prediction` paths in SSM commands to Windows paths
**Why it happens:** MSYS path conversion is automatic in Git Bash
**How to avoid:** Always prefix SSM commands with `MSYS_NO_PATHCONV=1`

### Pitfall 3: Polymarket WS May Not Connect
**What goes wrong:** Polymarket WebSocket shows "Connection reset by peer" from EC2
**Why it happens:** Known issue documented in Phase 40 -- possible geo/infra restriction
**How to avoid:** The system should fall back to REST polling automatically (Phase 42 SourceCoordinator). Verify `feed_source_mode` metric shows REST mode if WS fails.

### Pitfall 4: No Active Events Mapped
**What goes wrong:** `arb_events_tracked` is 0, no signals generated even though feeds are up
**Why it happens:** No events in `config/events.toml` match currently active Polymarket markets with corresponding Deribit/Derive options
**How to avoid:** Check events.toml has at least one active event with valid instrument IDs. May need to update events.toml on EC2 config.

### Pitfall 5: Staleness Rejections Block All Signals
**What goes wrong:** `arb_staleness_rejections` is high, `arb_signals_emitted_total` is 0
**Why it happens:** Options data staleness threshold (30s default) or prediction market staleness threshold (5s default) is too tight for the actual update frequency
**How to avoid:** Monitor staleness rejection rate in Grafana. If too high, consider adjusting `options_staleness_ms` or `polymarket_staleness_ms` in config.

## Verification Steps (Recommended Plan Structure)

### Pre-Deploy Fix: Signal Logs Volume Mount

Files to modify:
1. `docker-compose.yml` -- add `- /opt/prediction/data/signal_logs:/app/signal_logs` under prediction service volumes
2. `infra/cdk/lib/prediction-stack.ts` -- add `signal_logs` to:
   - Data directory creation: `mkdir -p /opt/prediction/data/{...,signal_logs}`
   - Docker-compose volumes: `- /opt/prediction/data/signal_logs:/app/signal_logs`

### Deploy and Verify Sequence

1. Fix signal_logs volume mount (code change)
2. Push to master (triggers CI/CD deploy)
3. Wait for deploy to succeed (health check passes)
4. Verify feeds are up: check `feed_available` metrics in Grafana or via SSM
5. Verify source mode: check `feed_source_mode` for Polymarket (WS or REST)
6. Wait for signal generation (may take minutes for first computation cycle)
7. Check Grafana signal-quality dashboard for non-zero metrics
8. Check signal JSONL logs via SSM: `docker exec prediction-prediction-1 cat /app/signal_logs/$(date +%Y-%m-%d).jsonl | head -5`
9. Check spread JSONL logs via SSM: same pattern with `/app/spread_logs/`
10. Verify JSONL entries have correct venue attribution (options_leg.venue, prediction_venue)

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hardcoded Deribit-only signals | Venue-generic CrossAssetEngine (Deribit + Derive) | Phase 41 | Signals can come from either options venue |
| WS-only Polymarket data | WS + REST fallback with SourceCoordinator | Phase 42 | Data flows even if WS is blocked |
| No signal JSONL logging on host | Signal JSONL persisted to host volume | Phase 43 (fix needed) | Logs survive container restarts |

## Open Questions

1. **Are there active events with valid instruments?**
   - What we know: events.toml has event definitions, but we haven't verified they map to currently tradeable instruments
   - What's unclear: Whether the events configured have active Polymarket markets AND corresponding Deribit/Derive options right now
   - Recommendation: Check events.toml content during verification; may need to add/update an event

2. **Will Polymarket data actually flow from EC2?**
   - What we know: Phase 40 diagnostic test exists but hasn't been run from production yet; REST fallback exists
   - What's unclear: Whether WS or REST will be the active mode
   - Recommendation: Accept either mode -- the SourceCoordinator handles this automatically. Just verify `feed_source_mode` shows which mode is active.

## Sources

### Primary (HIGH confidence)
- `docker-compose.yml` -- verified volume mounts (signal_logs missing)
- `infra/cdk/lib/prediction-stack.ts` -- verified CDK user-data docker-compose generation (signal_logs missing)
- `src/signal/config.rs` -- verified log_dir default is "signal_logs"
- `src/signal/logger.rs` -- verified JSONL write pattern
- `src/spread/logger.rs` -- verified JSONL write pattern
- `grafana/provisioning/dashboards/json/signal-quality.json` -- verified dashboard panel metrics
- `.gitlab-ci.yml` -- verified deploy pipeline structure

### Secondary (MEDIUM confidence)
- Phase 40 verification report -- confirmed SSM command patterns and diagnostic test

## Metadata

**Confidence breakdown:**
- Infrastructure gap (signal_logs mount): HIGH -- directly verified in source files
- Grafana dashboard metrics: HIGH -- verified in dashboard JSON
- Deployment flow: HIGH -- verified in .gitlab-ci.yml and CDK
- Signal generation pipeline: HIGH -- verified metric emissions in engine code
- Event availability: MEDIUM -- depends on runtime state of external markets

**Research date:** 2026-03-09
**Valid until:** 2026-03-23 (14 days -- production environment may drift)
